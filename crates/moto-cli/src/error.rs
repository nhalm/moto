//! CLI error types with exit codes.

use std::fmt;

/// Exit codes for the CLI (per moto-cli.md spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Success (0).
    Success = 0,
    /// General error (1).
    General = 1,
    /// Resource not found (2).
    NotFound = 2,
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

    /// Create a not-found error (exit code 2).
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            exit_code: ExitCode::NotFound,
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

/// Convert `moto_k8s` errors to CLI errors with appropriate exit codes.
impl From<moto_k8s::Error> for CliError {
    fn from(err: moto_k8s::Error) -> Self {
        match &err {
            moto_k8s::Error::NamespaceNotFound(_)
            | moto_k8s::Error::PodNotFound(_)
            | moto_k8s::Error::ContextNotFound(_)
            | moto_k8s::Error::DeploymentNotFound(_) => Self::not_found(err.to_string()),
            _ => Self::general(err.to_string()),
        }
    }
}

/// Convert Box<dyn Error> to CLI error (defaults to general error).
impl From<Box<dyn std::error::Error>> for CliError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Self::general(err.to_string())
    }
}

/// Convert `serde_json` errors to CLI error.
impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        Self::general(err.to_string())
    }
}

/// Convert io errors to CLI error.
impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::general(err.to_string())
    }
}

/// Convert &str to CLI error (general error).
impl From<&str> for CliError {
    fn from(msg: &str) -> Self {
        Self::general(msg)
    }
}

/// Convert String to CLI error (general error).
impl From<String> for CliError {
    fn from(msg: String) -> Self {
        Self::general(msg)
    }
}

/// Result type alias for CLI operations.
pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_values() {
        assert_eq!(i32::from(ExitCode::Success), 0);
        assert_eq!(i32::from(ExitCode::General), 1);
        assert_eq!(i32::from(ExitCode::NotFound), 2);
        assert_eq!(i32::from(ExitCode::InvalidInput), 3);
    }

    #[test]
    fn test_cli_error_general() {
        let err = CliError::general("test error");
        assert_eq!(err.exit_code, ExitCode::General);
        assert_eq!(err.message, "test error");
    }

    #[test]
    fn test_cli_error_not_found() {
        let err = CliError::not_found("garage not found");
        assert_eq!(err.exit_code, ExitCode::NotFound);
        assert_eq!(err.message, "garage not found");
    }

    #[test]
    fn test_cli_error_invalid_input() {
        let err = CliError::invalid_input("bad input");
        assert_eq!(err.exit_code, ExitCode::InvalidInput);
        assert_eq!(err.message, "bad input");
    }

    #[test]
    fn test_cli_error_display() {
        let err = CliError::general("display test");
        assert_eq!(format!("{err}"), "display test");
    }
}
