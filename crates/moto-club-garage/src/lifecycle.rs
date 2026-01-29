//! Garage lifecycle state transitions.
//!
//! Defines valid state transitions for garages and provides
//! validation for state changes.

use moto_club_db::GarageStatus;
use thiserror::Error;

/// Errors from lifecycle operations.
#[derive(Debug, Error)]
pub enum LifecycleError {
    /// Invalid state transition.
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition {
        /// Current state.
        from: GarageStatus,
        /// Attempted target state.
        to: GarageStatus,
    },

    /// Garage is already terminated.
    #[error("garage is already terminated")]
    AlreadyTerminated,

    /// Garage has expired.
    #[error("garage has expired")]
    Expired,
}

/// Garage lifecycle state machine.
///
/// Validates state transitions according to the garage lifecycle rules:
///
/// ```text
/// Pending → Running → Ready
///    ↓         ↓        ↓
///    └─────────┴────────┴──→ Terminated
/// ```
///
/// - Any state can transition to `Terminated`
/// - `Pending` can transition to `Running`
/// - `Running` can transition to `Ready`
///
/// Note: `Attached` status was removed in spec v1.1 (no mechanism to detect WireGuard connection).
pub struct GarageLifecycle;

impl GarageLifecycle {
    /// Checks if a state transition is valid.
    ///
    /// # Returns
    ///
    /// `true` if the transition is allowed, `false` otherwise.
    #[must_use]
    pub fn can_transition(from: GarageStatus, to: GarageStatus) -> bool {
        // Same state is always valid (no-op)
        if from == to {
            return true;
        }

        // Any state can go to Terminated
        if to == GarageStatus::Terminated {
            return from != GarageStatus::Terminated;
        }

        // Cannot transition from Terminated
        if from == GarageStatus::Terminated {
            return false;
        }

        matches!(
            (from, to),
            // Forward progress
            (GarageStatus::Pending, GarageStatus::Running)
                | (GarageStatus::Running, GarageStatus::Ready)
        )
    }

    /// Validates a state transition, returning an error if invalid.
    ///
    /// # Errors
    ///
    /// Returns `LifecycleError::InvalidTransition` if the transition is not allowed.
    /// Returns `LifecycleError::AlreadyTerminated` if trying to transition from Terminated.
    pub fn validate_transition(from: GarageStatus, to: GarageStatus) -> Result<(), LifecycleError> {
        if from == GarageStatus::Terminated {
            return Err(LifecycleError::AlreadyTerminated);
        }

        if Self::can_transition(from, to) {
            Ok(())
        } else {
            Err(LifecycleError::InvalidTransition { from, to })
        }
    }

    /// Checks if a garage in the given state can be extended.
    ///
    /// TTL can be extended for garages that are not terminated.
    #[must_use]
    pub fn can_extend_ttl(status: GarageStatus) -> bool {
        status != GarageStatus::Terminated
    }

    /// Checks if a garage in the given state can be closed.
    ///
    /// Any non-terminated garage can be closed.
    #[must_use]
    pub fn can_close(status: GarageStatus) -> bool {
        status != GarageStatus::Terminated
    }

    /// Returns the next expected state for a garage in the given state.
    ///
    /// Used for reconciliation to understand expected forward progress.
    #[must_use]
    pub const fn next_state(current: GarageStatus) -> Option<GarageStatus> {
        match current {
            GarageStatus::Pending => Some(GarageStatus::Running),
            GarageStatus::Running => Some(GarageStatus::Ready),
            GarageStatus::Ready | GarageStatus::Terminated => None,
        }
    }

    /// Checks if a state is terminal (no forward progress expected).
    #[must_use]
    pub const fn is_terminal(status: GarageStatus) -> bool {
        matches!(status, GarageStatus::Ready | GarageStatus::Terminated)
    }

    /// Checks if a state is active (not terminated).
    #[must_use]
    pub fn is_active(status: GarageStatus) -> bool {
        status != GarageStatus::Terminated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_state_transition_is_valid() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Running,
            GarageStatus::Ready,
            GarageStatus::Terminated,
        ] {
            assert!(GarageLifecycle::can_transition(status, status));
        }
    }

    #[test]
    fn forward_transitions_are_valid() {
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Pending,
            GarageStatus::Running
        ));
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Running,
            GarageStatus::Ready
        ));
    }

    #[test]
    fn any_state_can_terminate() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Running,
            GarageStatus::Ready,
        ] {
            assert!(GarageLifecycle::can_transition(
                status,
                GarageStatus::Terminated
            ));
        }
    }

    #[test]
    fn cannot_transition_from_terminated() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Running,
            GarageStatus::Ready,
        ] {
            assert!(!GarageLifecycle::can_transition(
                GarageStatus::Terminated,
                status
            ));
        }
    }

    #[test]
    fn backward_transitions_are_invalid() {
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Running,
            GarageStatus::Pending
        ));
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Ready,
            GarageStatus::Running
        ));
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Ready,
            GarageStatus::Pending
        ));
    }

    #[test]
    fn skip_transitions_are_invalid() {
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Pending,
            GarageStatus::Ready
        ));
    }

    #[test]
    fn validate_transition_returns_appropriate_error() {
        // Invalid transition
        let result =
            GarageLifecycle::validate_transition(GarageStatus::Ready, GarageStatus::Pending);
        assert!(matches!(
            result,
            Err(LifecycleError::InvalidTransition { .. })
        ));

        // From terminated
        let result =
            GarageLifecycle::validate_transition(GarageStatus::Terminated, GarageStatus::Ready);
        assert!(matches!(result, Err(LifecycleError::AlreadyTerminated)));

        // Valid transition
        let result =
            GarageLifecycle::validate_transition(GarageStatus::Pending, GarageStatus::Running);
        assert!(result.is_ok());
    }

    #[test]
    fn can_extend_ttl_for_active_garages() {
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Pending));
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Running));
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Ready));
        assert!(!GarageLifecycle::can_extend_ttl(GarageStatus::Terminated));
    }

    #[test]
    fn can_close_active_garages() {
        assert!(GarageLifecycle::can_close(GarageStatus::Pending));
        assert!(GarageLifecycle::can_close(GarageStatus::Running));
        assert!(GarageLifecycle::can_close(GarageStatus::Ready));
        assert!(!GarageLifecycle::can_close(GarageStatus::Terminated));
    }

    #[test]
    fn next_state_returns_expected_progression() {
        assert_eq!(
            GarageLifecycle::next_state(GarageStatus::Pending),
            Some(GarageStatus::Running)
        );
        assert_eq!(
            GarageLifecycle::next_state(GarageStatus::Running),
            Some(GarageStatus::Ready)
        );
        assert_eq!(GarageLifecycle::next_state(GarageStatus::Ready), None);
        assert_eq!(GarageLifecycle::next_state(GarageStatus::Terminated), None);
    }

    #[test]
    fn is_terminal_identifies_terminal_states() {
        assert!(!GarageLifecycle::is_terminal(GarageStatus::Pending));
        assert!(!GarageLifecycle::is_terminal(GarageStatus::Running));
        assert!(GarageLifecycle::is_terminal(GarageStatus::Ready));
        assert!(GarageLifecycle::is_terminal(GarageStatus::Terminated));
    }

    #[test]
    fn is_active_identifies_active_states() {
        assert!(GarageLifecycle::is_active(GarageStatus::Pending));
        assert!(GarageLifecycle::is_active(GarageStatus::Running));
        assert!(GarageLifecycle::is_active(GarageStatus::Ready));
        assert!(!GarageLifecycle::is_active(GarageStatus::Terminated));
    }
}
