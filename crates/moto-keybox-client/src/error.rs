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
}
