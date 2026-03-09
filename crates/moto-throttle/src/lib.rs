//! Rate limiting library for moto services.
//!
//! Provides per-principal request throttling using a token bucket algorithm,
//! exposed as a tower middleware layer.

mod config;
mod layer;
mod token_bucket;

#[cfg(test)]
mod token_bucket_test;

pub use config::{PrincipalType, ThrottleConfig, TierConfig};
pub use layer::{Principal, ThrottleLayer, ThrottleService};
pub use token_bucket::{CheckResult, TokenBucket};
