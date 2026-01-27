//! Error types for moto-keybox.

use thiserror::Error;

/// Error type for keybox operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Authentication failed.
    #[error("authentication failed: {message}")]
    Auth {
        /// Details about the authentication failure.
        message: String,
    },

    /// Authorization denied by ABAC policy.
    #[error("access denied: {message}")]
    AccessDenied {
        /// Details about the policy violation.
        message: String,
    },

    /// Secret not found.
    #[error("secret not found: {scope}/{name}")]
    SecretNotFound {
        /// The scope that was searched.
        scope: String,
        /// The secret name that was not found.
        name: String,
    },

    /// Secret already exists.
    #[error("secret already exists: {scope}/{name}")]
    SecretExists {
        /// The scope where the secret exists.
        scope: String,
        /// The secret name that already exists.
        name: String,
    },

    /// Invalid SPIFFE ID format.
    #[error("invalid SPIFFE ID: {id}")]
    InvalidSpiffeId {
        /// The malformed SPIFFE ID.
        id: String,
    },

    /// SVID has expired.
    #[error("SVID expired")]
    SvidExpired,

    /// Invalid SVID signature.
    #[error("invalid SVID signature")]
    InvalidSvidSignature,

    /// Cryptographic operation failed.
    #[error("crypto error: {message}")]
    Crypto {
        /// Details about the crypto failure.
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
}

/// Result type alias using the keybox Error.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_auth() {
        let err = Error::Auth {
            message: "invalid token".to_string(),
        };
        assert_eq!(err.to_string(), "authentication failed: invalid token");
    }

    #[test]
    fn error_display_access_denied() {
        let err = Error::AccessDenied {
            message: "policy violation".to_string(),
        };
        assert_eq!(err.to_string(), "access denied: policy violation");
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
    fn error_display_secret_exists() {
        let err = Error::SecretExists {
            scope: "service".to_string(),
            name: "db/password".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "secret already exists: service/db/password"
        );
    }

    #[test]
    fn error_display_invalid_spiffe_id() {
        let err = Error::InvalidSpiffeId {
            id: "bad-id".to_string(),
        };
        assert_eq!(err.to_string(), "invalid SPIFFE ID: bad-id");
    }

    #[test]
    fn error_display_svid_expired() {
        let err = Error::SvidExpired;
        assert_eq!(err.to_string(), "SVID expired");
    }

    #[test]
    fn error_display_invalid_signature() {
        let err = Error::InvalidSvidSignature;
        assert_eq!(err.to_string(), "invalid SVID signature");
    }
}
