//! Low-level Kubernetes operations for moto.
//!
//! This crate provides:
//! - [`K8sClient`] - A wrapper around `kube::Client` for K8s operations
//! - [`NamespaceOps`] - Trait for namespace CRUD operations
//! - [`PodOps`] - Trait for pod operations (list, logs)
//! - [`labels`] - Constants for moto K8s labels

mod client;
mod error;
mod labels;
mod namespace;
mod pod;

pub use client::K8sClient;
pub use error::{Error, Result};
pub use labels::Labels;
pub use namespace::NamespaceOps;
pub use pod::{LogStream, PodLogOptions, PodOps};
