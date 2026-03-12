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
/// Pending → Initializing → Ready
///    ↓            ↓          ↓
///    └────────────┼──────────┴──→ Terminated
///                 ↓
///              Failed ──────────→ Terminated
/// ```
///
/// - Any non-terminal state can transition to `Terminated`
/// - `Pending` can transition to `Initializing` or `Failed`
/// - `Initializing` can transition to `Ready` or `Failed`
/// - `Failed` can only transition to `Terminated`
///
/// See garage-lifecycle.md v0.3 for the 5-state model.
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

        // Cannot transition from terminal states (except Failed -> Terminated)
        if from == GarageStatus::Terminated {
            return false;
        }

        // Failed can only go to Terminated
        if from == GarageStatus::Failed {
            return to == GarageStatus::Terminated;
        }

        // Any active state can go to Terminated
        if to == GarageStatus::Terminated {
            return true;
        }

        matches!(
            (from, to),
            // Forward progress and failure transitions
            (
                GarageStatus::Pending,
                GarageStatus::Initializing | GarageStatus::Failed
            ) | (
                GarageStatus::Initializing,
                GarageStatus::Ready | GarageStatus::Failed
            )
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
    /// TTL can be extended for garages that are not in terminal states.
    #[must_use]
    pub const fn can_extend_ttl(status: GarageStatus) -> bool {
        !matches!(status, GarageStatus::Terminated | GarageStatus::Failed)
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
            GarageStatus::Pending => Some(GarageStatus::Initializing),
            GarageStatus::Initializing => Some(GarageStatus::Ready),
            GarageStatus::Ready | GarageStatus::Failed | GarageStatus::Terminated => None,
        }
    }

    /// Checks if a state is terminal (no forward progress expected).
    #[must_use]
    pub const fn is_terminal(status: GarageStatus) -> bool {
        matches!(status, GarageStatus::Failed | GarageStatus::Terminated)
    }

    /// Checks if a state is active (not in a terminal failure or terminated state).
    #[must_use]
    pub const fn is_active(status: GarageStatus) -> bool {
        !matches!(status, GarageStatus::Terminated | GarageStatus::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_state_transition_is_valid() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Initializing,
            GarageStatus::Ready,
            GarageStatus::Failed,
            GarageStatus::Terminated,
        ] {
            assert!(GarageLifecycle::can_transition(status, status));
        }
    }

    #[test]
    fn forward_transitions_are_valid() {
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Pending,
            GarageStatus::Initializing
        ));
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Initializing,
            GarageStatus::Ready
        ));
    }

    #[test]
    fn failure_transitions_are_valid() {
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Pending,
            GarageStatus::Failed
        ));
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Initializing,
            GarageStatus::Failed
        ));
    }

    #[test]
    fn any_active_state_can_terminate() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Initializing,
            GarageStatus::Ready,
            GarageStatus::Failed,
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
            GarageStatus::Initializing,
            GarageStatus::Ready,
            GarageStatus::Failed,
        ] {
            assert!(!GarageLifecycle::can_transition(
                GarageStatus::Terminated,
                status
            ));
        }
    }

    #[test]
    fn failed_can_only_go_to_terminated() {
        // Failed cannot transition to normal states
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Failed,
            GarageStatus::Pending
        ));
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Failed,
            GarageStatus::Initializing
        ));
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Failed,
            GarageStatus::Ready
        ));
        // But can terminate
        assert!(GarageLifecycle::can_transition(
            GarageStatus::Failed,
            GarageStatus::Terminated
        ));
    }

    #[test]
    fn backward_transitions_are_invalid() {
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Initializing,
            GarageStatus::Pending
        ));
        assert!(!GarageLifecycle::can_transition(
            GarageStatus::Ready,
            GarageStatus::Initializing
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
            GarageLifecycle::validate_transition(GarageStatus::Pending, GarageStatus::Initializing);
        assert!(result.is_ok());
    }

    #[test]
    fn can_extend_ttl_for_active_garages() {
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Pending));
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Initializing));
        assert!(GarageLifecycle::can_extend_ttl(GarageStatus::Ready));
        assert!(!GarageLifecycle::can_extend_ttl(GarageStatus::Failed));
        assert!(!GarageLifecycle::can_extend_ttl(GarageStatus::Terminated));
    }

    #[test]
    fn can_close_any_non_terminated_garage() {
        assert!(GarageLifecycle::can_close(GarageStatus::Pending));
        assert!(GarageLifecycle::can_close(GarageStatus::Initializing));
        assert!(GarageLifecycle::can_close(GarageStatus::Ready));
        assert!(GarageLifecycle::can_close(GarageStatus::Failed));
        assert!(!GarageLifecycle::can_close(GarageStatus::Terminated));
    }

    #[test]
    fn next_state_returns_expected_progression() {
        assert_eq!(
            GarageLifecycle::next_state(GarageStatus::Pending),
            Some(GarageStatus::Initializing)
        );
        assert_eq!(
            GarageLifecycle::next_state(GarageStatus::Initializing),
            Some(GarageStatus::Ready)
        );
        assert_eq!(GarageLifecycle::next_state(GarageStatus::Ready), None);
        assert_eq!(GarageLifecycle::next_state(GarageStatus::Failed), None);
        assert_eq!(GarageLifecycle::next_state(GarageStatus::Terminated), None);
    }

    #[test]
    fn is_terminal_identifies_terminal_states() {
        assert!(!GarageLifecycle::is_terminal(GarageStatus::Pending));
        assert!(!GarageLifecycle::is_terminal(GarageStatus::Initializing));
        assert!(!GarageLifecycle::is_terminal(GarageStatus::Ready));
        assert!(GarageLifecycle::is_terminal(GarageStatus::Failed));
        assert!(GarageLifecycle::is_terminal(GarageStatus::Terminated));
    }

    #[test]
    fn is_active_identifies_active_states() {
        assert!(GarageLifecycle::is_active(GarageStatus::Pending));
        assert!(GarageLifecycle::is_active(GarageStatus::Initializing));
        assert!(GarageLifecycle::is_active(GarageStatus::Ready));
        assert!(!GarageLifecycle::is_active(GarageStatus::Failed));
        assert!(!GarageLifecycle::is_active(GarageStatus::Terminated));
    }
}
