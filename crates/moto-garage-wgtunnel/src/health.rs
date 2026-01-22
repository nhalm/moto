//! Health endpoint for the garage wgtunnel daemon.
//!
//! Provides a simple HTTP health endpoint for Kubernetes liveness/readiness probes
//! and operational monitoring.
//!
//! # Endpoint
//!
//! ```text
//! GET /health
//!
//! Response 200:
//! {
//!   "status": "healthy",
//!   "wireguard": "up",
//!   "moto_club_connected": true,
//!   "active_peers": 2
//! }
//! ```
//!
//! # Example
//!
//! ```ignore
//! use moto_garage_wgtunnel::health::{HealthCheck, HealthStatus, WireGuardState};
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let health = HealthCheck::new();
//!
//! // Update state as things change
//! health.set_wireguard_state(WireGuardState::Up);
//! health.set_moto_club_connected(true);
//! health.set_active_peers(2);
//!
//! // Get current status
//! let status = health.status();
//! assert!(status.is_healthy());
//! # Ok(())
//! # }
//! ```

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// `WireGuard` tunnel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireGuardState {
    /// Tunnel is up and operational.
    Up,

    /// Tunnel is down or not yet initialized.
    #[default]
    Down,

    /// Tunnel is in an error state.
    Error,
}

impl WireGuardState {
    /// Check if the `WireGuard` tunnel is operational.
    #[must_use]
    pub const fn is_up(&self) -> bool {
        matches!(self, Self::Up)
    }

    /// Convert to a string representation for the API.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Error => "error",
        }
    }
}

/// Overall health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverallStatus {
    /// All systems operational.
    Healthy,

    /// Some non-critical systems degraded.
    Degraded,

    /// Critical systems failed.
    Unhealthy,
}

impl OverallStatus {
    /// Convert to a string representation for the API.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unhealthy => "unhealthy",
        }
    }
}

/// Health status response returned by the health endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Overall health status.
    pub status: OverallStatus,

    /// `WireGuard` tunnel state.
    pub wireguard: WireGuardState,

    /// Whether connected to moto-club coordination server.
    pub moto_club_connected: bool,

    /// Number of currently active `WireGuard` peers.
    pub active_peers: u32,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            status: OverallStatus::Unhealthy,
            wireguard: WireGuardState::Down,
            moto_club_connected: false,
            active_peers: 0,
        }
    }
}

impl HealthStatus {
    /// Create a healthy status.
    #[must_use]
    pub const fn healthy(active_peers: u32) -> Self {
        Self {
            status: OverallStatus::Healthy,
            wireguard: WireGuardState::Up,
            moto_club_connected: true,
            active_peers,
        }
    }

    /// Check if the overall status is healthy.
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        matches!(self.status, OverallStatus::Healthy)
    }

    /// Check if the status indicates critical failure.
    #[must_use]
    pub const fn is_unhealthy(&self) -> bool {
        matches!(self.status, OverallStatus::Unhealthy)
    }

    /// Get the appropriate HTTP status code for this health status.
    #[must_use]
    pub const fn http_status_code(&self) -> u16 {
        match self.status {
            // Both healthy and degraded return 200 - degraded can still serve existing peers
            OverallStatus::Healthy | OverallStatus::Degraded => 200,
            OverallStatus::Unhealthy => 503,
        }
    }

    /// Compute overall status from component states.
    const fn compute_status(wireguard: WireGuardState, moto_club_connected: bool) -> OverallStatus {
        match (wireguard, moto_club_connected) {
            // Healthy: WireGuard up and connected to moto-club
            (WireGuardState::Up, true) => OverallStatus::Healthy,

            // Degraded: WireGuard up but moto-club disconnected
            // (can still serve existing peers, but won't get new ones)
            (WireGuardState::Up, false) => OverallStatus::Degraded,

            // Unhealthy: WireGuard down or error
            (WireGuardState::Down | WireGuardState::Error, _) => OverallStatus::Unhealthy,
        }
    }
}

/// Thread-safe health check state.
///
/// This struct is designed to be shared across async tasks and updated
/// from multiple sources (`WireGuard` engine, moto-club client, etc.).
#[derive(Debug)]
pub struct HealthCheck {
    /// Encoded `WireGuard` state (0=Down, 1=Up, 2=Error).
    wireguard_state: AtomicU32,

    /// Whether connected to moto-club.
    moto_club_connected: AtomicBool,

    /// Number of active peers.
    active_peers: AtomicU32,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthCheck {
    /// Create a new health check with default (unhealthy) state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wireguard_state: AtomicU32::new(0), // Down
            moto_club_connected: AtomicBool::new(false),
            active_peers: AtomicU32::new(0),
        }
    }

    /// Update the `WireGuard` tunnel state.
    pub fn set_wireguard_state(&self, state: WireGuardState) {
        let encoded = match state {
            WireGuardState::Down => 0,
            WireGuardState::Up => 1,
            WireGuardState::Error => 2,
        };
        self.wireguard_state.store(encoded, Ordering::Release);
    }

    /// Get the current `WireGuard` tunnel state.
    #[must_use]
    pub fn wireguard_state(&self) -> WireGuardState {
        match self.wireguard_state.load(Ordering::Acquire) {
            1 => WireGuardState::Up,
            2 => WireGuardState::Error,
            _ => WireGuardState::Down,
        }
    }

    /// Update the moto-club connection status.
    pub fn set_moto_club_connected(&self, connected: bool) {
        self.moto_club_connected.store(connected, Ordering::Release);
    }

    /// Check if connected to moto-club.
    #[must_use]
    pub fn moto_club_connected(&self) -> bool {
        self.moto_club_connected.load(Ordering::Acquire)
    }

    /// Update the active peer count.
    pub fn set_active_peers(&self, count: u32) {
        self.active_peers.store(count, Ordering::Release);
    }

    /// Increment the active peer count.
    ///
    /// Returns the new count.
    pub fn increment_peers(&self) -> u32 {
        self.active_peers.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Decrement the active peer count (saturating at 0).
    ///
    /// Returns the new count.
    pub fn decrement_peers(&self) -> u32 {
        // Use a loop to handle the saturating decrement
        loop {
            let current = self.active_peers.load(Ordering::Acquire);
            if current == 0 {
                return 0;
            }
            if self
                .active_peers
                .compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return current - 1;
            }
        }
    }

    /// Get the current active peer count.
    #[must_use]
    pub fn active_peers(&self) -> u32 {
        self.active_peers.load(Ordering::Acquire)
    }

    /// Get the current health status.
    #[must_use]
    pub fn status(&self) -> HealthStatus {
        let wireguard = self.wireguard_state();
        let moto_club_connected = self.moto_club_connected();
        let active_peers = self.active_peers();

        HealthStatus {
            status: HealthStatus::compute_status(wireguard, moto_club_connected),
            wireguard,
            moto_club_connected,
            active_peers,
        }
    }

    /// Check if the daemon is healthy (for quick checks).
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.wireguard_state().is_up() && self.moto_club_connected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wireguard_state_is_up() {
        assert!(WireGuardState::Up.is_up());
        assert!(!WireGuardState::Down.is_up());
        assert!(!WireGuardState::Error.is_up());
    }

    #[test]
    fn wireguard_state_as_str() {
        assert_eq!(WireGuardState::Up.as_str(), "up");
        assert_eq!(WireGuardState::Down.as_str(), "down");
        assert_eq!(WireGuardState::Error.as_str(), "error");
    }

    #[test]
    fn overall_status_as_str() {
        assert_eq!(OverallStatus::Healthy.as_str(), "healthy");
        assert_eq!(OverallStatus::Degraded.as_str(), "degraded");
        assert_eq!(OverallStatus::Unhealthy.as_str(), "unhealthy");
    }

    #[test]
    fn health_status_default() {
        let status = HealthStatus::default();

        assert!(status.is_unhealthy());
        assert!(!status.is_healthy());
        assert_eq!(status.wireguard, WireGuardState::Down);
        assert!(!status.moto_club_connected);
        assert_eq!(status.active_peers, 0);
    }

    #[test]
    fn health_status_healthy() {
        let status = HealthStatus::healthy(5);

        assert!(status.is_healthy());
        assert!(!status.is_unhealthy());
        assert_eq!(status.wireguard, WireGuardState::Up);
        assert!(status.moto_club_connected);
        assert_eq!(status.active_peers, 5);
    }

    #[test]
    fn health_status_http_codes() {
        assert_eq!(HealthStatus::healthy(0).http_status_code(), 200);

        let degraded = HealthStatus {
            status: OverallStatus::Degraded,
            ..Default::default()
        };
        assert_eq!(degraded.http_status_code(), 200);

        let unhealthy = HealthStatus::default();
        assert_eq!(unhealthy.http_status_code(), 503);
    }

    #[test]
    fn health_status_compute_status() {
        // Healthy: WG up + moto-club connected
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Up, true),
            OverallStatus::Healthy
        );

        // Degraded: WG up but moto-club disconnected
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Up, false),
            OverallStatus::Degraded
        );

        // Unhealthy: WG down
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Down, true),
            OverallStatus::Unhealthy
        );
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Down, false),
            OverallStatus::Unhealthy
        );

        // Unhealthy: WG error
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Error, true),
            OverallStatus::Unhealthy
        );
        assert_eq!(
            HealthStatus::compute_status(WireGuardState::Error, false),
            OverallStatus::Unhealthy
        );
    }

    #[test]
    fn health_status_serde() {
        let status = HealthStatus::healthy(3);
        let json = serde_json::to_string(&status).unwrap();

        assert!(json.contains("\"status\":\"healthy\""));
        assert!(json.contains("\"wireguard\":\"up\""));
        assert!(json.contains("\"moto_club_connected\":true"));
        assert!(json.contains("\"active_peers\":3"));

        let parsed: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, status.status);
        assert_eq!(parsed.wireguard, status.wireguard);
        assert_eq!(parsed.moto_club_connected, status.moto_club_connected);
        assert_eq!(parsed.active_peers, status.active_peers);
    }

    #[test]
    fn health_check_new() {
        let check = HealthCheck::new();

        assert_eq!(check.wireguard_state(), WireGuardState::Down);
        assert!(!check.moto_club_connected());
        assert_eq!(check.active_peers(), 0);
        assert!(!check.is_healthy());
    }

    #[test]
    fn health_check_wireguard_state() {
        let check = HealthCheck::new();

        check.set_wireguard_state(WireGuardState::Up);
        assert_eq!(check.wireguard_state(), WireGuardState::Up);

        check.set_wireguard_state(WireGuardState::Error);
        assert_eq!(check.wireguard_state(), WireGuardState::Error);

        check.set_wireguard_state(WireGuardState::Down);
        assert_eq!(check.wireguard_state(), WireGuardState::Down);
    }

    #[test]
    fn health_check_moto_club_connection() {
        let check = HealthCheck::new();

        assert!(!check.moto_club_connected());

        check.set_moto_club_connected(true);
        assert!(check.moto_club_connected());

        check.set_moto_club_connected(false);
        assert!(!check.moto_club_connected());
    }

    #[test]
    fn health_check_active_peers() {
        let check = HealthCheck::new();

        check.set_active_peers(5);
        assert_eq!(check.active_peers(), 5);

        // Test increment
        assert_eq!(check.increment_peers(), 6);
        assert_eq!(check.active_peers(), 6);

        // Test decrement
        assert_eq!(check.decrement_peers(), 5);
        assert_eq!(check.active_peers(), 5);
    }

    #[test]
    fn health_check_decrement_saturates_at_zero() {
        let check = HealthCheck::new();

        assert_eq!(check.active_peers(), 0);
        assert_eq!(check.decrement_peers(), 0);
        assert_eq!(check.active_peers(), 0);

        check.set_active_peers(1);
        assert_eq!(check.decrement_peers(), 0);
        assert_eq!(check.decrement_peers(), 0);
    }

    #[test]
    fn health_check_status() {
        let check = HealthCheck::new();

        // Initially unhealthy
        let status = check.status();
        assert!(status.is_unhealthy());
        assert_eq!(status.wireguard, WireGuardState::Down);
        assert!(!status.moto_club_connected);
        assert_eq!(status.active_peers, 0);

        // Set up healthy state
        check.set_wireguard_state(WireGuardState::Up);
        check.set_moto_club_connected(true);
        check.set_active_peers(2);

        let status = check.status();
        assert!(status.is_healthy());
        assert_eq!(status.wireguard, WireGuardState::Up);
        assert!(status.moto_club_connected);
        assert_eq!(status.active_peers, 2);
    }

    #[test]
    fn health_check_is_healthy() {
        let check = HealthCheck::new();

        // Not healthy: WG down
        assert!(!check.is_healthy());

        // Not healthy: WG up, moto-club disconnected
        check.set_wireguard_state(WireGuardState::Up);
        assert!(!check.is_healthy());

        // Healthy: both up
        check.set_moto_club_connected(true);
        assert!(check.is_healthy());

        // Not healthy: WG error
        check.set_wireguard_state(WireGuardState::Error);
        assert!(!check.is_healthy());
    }

    #[test]
    fn health_check_thread_safe() {
        use std::sync::Arc;
        use std::thread;

        let check = Arc::new(HealthCheck::new());

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let check = Arc::clone(&check);
                thread::spawn(move || {
                    for _ in 0..100 {
                        check.increment_peers();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(check.active_peers(), 400);
    }

    #[test]
    fn health_check_default() {
        let check = HealthCheck::default();
        assert_eq!(check.active_peers(), 0);
        assert!(!check.is_healthy());
    }
}
