//! Rate limiting library for moto services.
//!
//! Provides per-principal request throttling using a token bucket algorithm.
//! Designed to be embedded as tower middleware in moto services.

mod token_bucket;
#[cfg(test)]
mod token_bucket_test;

pub use token_bucket::{CheckResult, TokenBucket};
