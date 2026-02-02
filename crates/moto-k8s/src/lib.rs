//! Low-level Kubernetes operations for moto.
//!
//! This crate provides:
//! - [`K8sClient`] - A wrapper around `kube::Client` for K8s operations
//! - [`NamespaceOps`] - Trait for namespace CRUD operations
//! - [`PodOps`] - Trait for pod operations (list, logs)
//! - [`DeploymentOps`] - Trait for deployment operations
//! - [`PvcOps`] - Trait for `PersistentVolumeClaim` operations
//! - [`NetworkPolicyOps`] - Trait for `NetworkPolicy` operations
//! - [`ResourceQuotaOps`] - Trait for `ResourceQuota` operations
//! - [`TokenReviewOps`] - Trait for `ServiceAccount` token validation
//! - [`labels`] - Constants for moto K8s labels

mod client;
mod deployment;
mod error;
mod labels;
mod namespace;
mod network_policy;
mod pod;
mod pvc;
mod resource_quota;
mod token_review;

pub use client::K8sClient;
pub use deployment::{BikeDeploymentConfig, BikeInfo, DeploymentOps};
pub use error::{Error, Result};
pub use labels::Labels;
pub use namespace::NamespaceOps;
pub use network_policy::NetworkPolicyOps;
pub use pod::{LogStream, PodLogOptions, PodOps};
pub use pvc::PvcOps;
pub use resource_quota::ResourceQuotaOps;
pub use token_review::{TokenReviewOps, ValidatedToken};
