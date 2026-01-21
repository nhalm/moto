//! Shared utilities for the moto monorepo.
//!
//! This crate provides common functionality used across all moto crates:
//! - Error types and result aliases
//! - Configuration loading
//! - `Secret<T>` wrapper for sensitive data

mod error;
mod secret;

pub use error::{Error, Result};
pub use secret::Secret;
