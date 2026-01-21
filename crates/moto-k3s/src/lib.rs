//! Low-level Kubernetes operations for moto.
//!
//! This crate provides:
//! - [`K3sClient`] - A wrapper around `kube::Client` for K8s operations
//! - [`NamespaceOps`] - Trait for namespace CRUD operations
//! - [`labels`] - Constants for moto K8s labels

mod client;
mod error;
mod labels;
mod namespace;

pub use client::K3sClient;
pub use error::{Error, Result};
pub use labels::Labels;
pub use namespace::NamespaceOps;
