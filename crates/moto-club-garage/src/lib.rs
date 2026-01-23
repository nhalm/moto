//! Garage service logic for moto-club.
//!
//! This crate provides the business logic layer for garage management,
//! coordinating between the database layer (`moto-club-db`) and the
//! Kubernetes layer (`moto-club-k8s`).
//!
//! # Example
//!
//! ```ignore
//! use moto_club_garage::{GarageService, CreateGarageInput};
//! use moto_club_db::DbPool;
//! use moto_club_k8s::GarageK8s;
//!
//! let db = DbPool::connect("postgres://...").await?;
//! let k8s = GarageK8s::new(K8sClient::new().await?);
//! let service = GarageService::new(db, k8s);
//!
//! // Create a garage
//! let input = CreateGarageInput {
//!     name: None, // auto-generate
//!     branch: "main".to_string(),
//!     ttl_seconds: Some(14400),
//!     image: None,
//! };
//! let garage = service.create("nick", input).await?;
//!
//! // Close a garage
//! service.close("nick", &garage.name).await?;
//! ```

mod lifecycle;
mod service;

pub use lifecycle::{GarageLifecycle, LifecycleError};
pub use service::{
    CreateGarageInput, ExtendTtlInput, GarageService, GarageServiceError,
};

/// Default TTL in seconds (4 hours).
pub const DEFAULT_TTL_SECONDS: i32 = 14400;

/// Maximum TTL in seconds (48 hours).
pub const MAX_TTL_SECONDS: i32 = 172_800;

/// Minimum TTL in seconds (5 minutes).
pub const MIN_TTL_SECONDS: i32 = 300;
