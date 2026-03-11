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
//! - [`LimitRangeOps`] - Trait for `LimitRange` operations
//! - [`TokenReviewOps`] - Trait for `ServiceAccount` token validation
//! - [`RbacOps`] - Trait for RBAC operations (Role, `RoleBinding`)
//! - [`labels`] - Constants for moto K8s labels

mod client;
mod deployment;
mod error;
mod labels;
mod limit_range;
mod namespace;
mod network_policy;
mod pod;
mod pvc;
mod rbac;
mod resource_quota;
mod token_review;

pub use client::K8sClient;
pub use deployment::{BikeDeploymentConfig, BikeInfo, DeploymentOps};
pub use error::{Error, Result};
pub use labels::Labels;
pub use limit_range::LimitRangeOps;
pub use namespace::NamespaceOps;
pub use network_policy::NetworkPolicyOps;
pub use pod::{LogStream, PodLogOptions, PodOps};
pub use pvc::PvcOps;
pub use rbac::RbacOps;
pub use resource_quota::ResourceQuotaOps;
pub use token_review::{TokenReviewOps, ValidatedToken};
