//! Client library for accessing secrets via moto-keybox.
//!
//! This crate provides a client for garages and bikes to fetch secrets from
//! the keybox service. It handles SVID (Short-lived Verifiable Identity Document)
//! caching and automatic refresh.
//!
//! # Modes of Operation
//!
//! The client supports two modes:
//!
//! - **K8s mode**: Fetches SVID by exchanging K8s `ServiceAccount` JWT (default)
//! - **Local mode**: Reads SVID from file specified by `MOTO_KEYBOX_SVID_FILE`
//!
//! # Example
//!
//! ```rust,no_run
//! use moto_keybox_client::{SvidCache, Scope};
//!
//! # async fn example() -> moto_keybox_client::Result<()> {
//! // Create cache from environment (auto-detects mode)
//! let cache = SvidCache::from_env().await?;
//!
//! // Get the current SVID token
//! let token = cache.get().await?;
//!
//! // Use token to authenticate to keybox server...
//! # Ok(())
//! # }
//! ```
//!
//! # Local Development
//!
//! For local development without K8s, set `MOTO_KEYBOX_SVID_FILE`:
//!
//! ```bash
//! # Issue a dev SVID using the CLI
//! moto keybox issue-dev-svid --garage-id=test-garage --output=./dev-svid.jwt
//!
//! # Set environment variable
//! export MOTO_KEYBOX_SVID_FILE=./dev-svid.jwt
//! ```
//!
//! The client will automatically load the SVID from the file.

mod error;
mod svid_cache;

pub use error::{Error, Result};
pub use svid_cache::SvidCache;

// Re-export commonly used types from moto-keybox
pub use moto_keybox::{
    AuditEntry, AuditEventType, PrincipalType, Scope, SecretMetadata, SpiffeId, SvidClaims,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexports_work() {
        // Verify re-exports are accessible
        let _scope = Scope::Global;
        let _principal = PrincipalType::Garage;
        let spiffe = SpiffeId::garage("test");
        assert_eq!(spiffe.to_uri(), "spiffe://moto.local/garage/test");
    }
}
