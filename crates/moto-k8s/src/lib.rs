//! Low-level Kubernetes operations for moto.
//!
//! This crate provides:
//! - [`K8sClient`] - A wrapper around `kube::Client` for K8s operations
//! - [`NamespaceOps`] - Trait for namespace CRUD operations
//! - [`PodOps`] - Trait for pod operations (list, logs)
//! - [`DeploymentOps`] - Trait for deployment operations
//! - [`PvcOps`] - Trait for `PersistentVolumeClaim` operations
//! - [`TokenReviewOps`] - Trait for `ServiceAccount` token validation
//! - [`labels`] - Constants for moto K8s labels

mod client;
mod deployment;
mod error;
mod labels;
mod namespace;
mod pod;
mod pvc;
mod token_review;

pub use client::K8sClient;
pub use deployment::{BikeDeploymentConfig, BikeInfo, DeploymentOps};
pub use error::{Error, Result};
pub use labels::Labels;
pub use namespace::NamespaceOps;
pub use pod::{LogStream, PodLogOptions, PodOps};
pub use pvc::PvcOps;
pub use token_review::{TokenReviewOps, ValidatedToken};
