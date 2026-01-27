//! Error types for moto-keybox-client.

use thiserror::Error;

/// Error type for keybox client operations.
#[derive(Debug, Error)]
pub enum Error {
    /// SVID has expired and could not be refreshed.
    #[error("SVID expired")]
    SvidExpired,

    /// No SVID available (not yet acquired or load failed).
    #[error("no SVID available: {message}")]
    NoSvid {
        /// Details about why no SVID is available.
        message: String,
    },

    /// Failed to load SVID from file.
    #[error("failed to load SVID: {message}")]
    SvidLoad {
        /// Details about the load failure.
        message: String,
    },

    /// Configuration error.
    #[error("configuration error: {message}")]
    Config {
        /// Details about the configuration problem.
        message: String,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Keybox server error (passed through).
    #[error("keybox error: {0}")]
    Keybox(#[from] moto_keybox::Error),

    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Keybox server returned an error.
    #[error("server error: {code} - {message}")]
    Server {
        /// Error code from the server.
        code: String,
        /// Error message from the server.
        message: String,
    },

    /// Secret not found.
    #[error("secret not found: {scope}/{name}")]
    SecretNotFound {
        /// The scope that was queried.
        scope: String,
        /// The secret name that was queried.
        name: String,
    },

    /// Access denied by ABAC policy.
    #[error("access denied: {message}")]
    AccessDenied {
        /// Details about why access was denied.
        message: String,
    },

    /// Keybox server unreachable.
    #[error("keybox unreachable at {url}: {reason}")]
    Unreachable {
        /// The URL that was attempted.
        url: String,
        /// Why it's unreachable.
        reason: String,
    },
}

/// Result type alias using the client Error.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_svid_expired() {
        let err = Error::SvidExpired;
        assert_eq!(err.to_string(), "SVID expired");
    }

    #[test]
    fn error_display_no_svid() {
        let err = Error::NoSvid {
            message: "not initialized".to_string(),
        };
        assert_eq!(err.to_string(), "no SVID available: not initialized");
    }

    #[test]
    fn error_display_svid_load() {
        let err = Error::SvidLoad {
            message: "file not found".to_string(),
        };
        assert_eq!(err.to_string(), "failed to load SVID: file not found");
    }

    #[test]
    fn error_display_config() {
        let err = Error::Config {
            message: "missing URL".to_string(),
        };
        assert_eq!(err.to_string(), "configuration error: missing URL");
    }

    #[test]
    fn error_display_server() {
        let err = Error::Server {
            code: "INTERNAL_ERROR".to_string(),
            message: "something went wrong".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "server error: INTERNAL_ERROR - something went wrong"
        );
    }

    #[test]
    fn error_display_secret_not_found() {
        let err = Error::SecretNotFound {
            scope: "global".to_string(),
            name: "ai/anthropic".to_string(),
        };
        assert_eq!(err.to_string(), "secret not found: global/ai/anthropic");
    }

    #[test]
    fn error_display_access_denied() {
        let err = Error::AccessDenied {
            message: "not authorized".to_string(),
        };
        assert_eq!(err.to_string(), "access denied: not authorized");
    }

    #[test]
    fn error_display_unreachable() {
        let err = Error::Unreachable {
            url: "http://localhost:8080".to_string(),
            reason: "connection refused".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "keybox unreachable at http://localhost:8080: connection refused"
        );
    }
}
