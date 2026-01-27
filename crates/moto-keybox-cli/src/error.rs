//! CLI error types with exit codes.

use std::fmt;

/// Exit codes for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Success (0).
    Success = 0,
    /// General error (1).
    General = 1,
    /// Invalid input (3).
    InvalidInput = 3,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as Self
    }
}

/// CLI error with an associated exit code.
#[derive(Debug)]
pub struct CliError {
    /// The error message.
    pub message: String,
    /// The exit code.
    pub exit_code: ExitCode,
}

impl CliError {
    /// Create a general error (exit code 1).
    pub fn general(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            exit_code: ExitCode::General,
        }
    }

    /// Create an invalid-input error (exit code 3).
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            exit_code: ExitCode::InvalidInput,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::general(err.to_string())
    }
}

impl From<moto_keybox::Error> for CliError {
    fn from(err: moto_keybox::Error) -> Self {
        Self::general(err.to_string())
    }
}

/// Result type alias for CLI operations.
pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_values() {
        assert_eq!(i32::from(ExitCode::Success), 0);
        assert_eq!(i32::from(ExitCode::General), 1);
        assert_eq!(i32::from(ExitCode::InvalidInput), 3);
    }

    #[test]
    fn cli_error_general() {
        let err = CliError::general("test error");
        assert_eq!(err.exit_code, ExitCode::General);
        assert_eq!(err.message, "test error");
    }

    #[test]
    fn cli_error_display() {
        let err = CliError::general("display test");
        assert_eq!(format!("{err}"), "display test");
    }
}
