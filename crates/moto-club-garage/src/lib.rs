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
//!     engine: None,
//!     repo: None,
//!     with_postgres: false,
//!     with_redis: false,
//! };
//! let garage = service.create("nick", input).await?;
//!
//! // Close a garage
//! service.close("nick", &garage.name).await?;
//! ```

mod keybox;
mod lifecycle;
mod service;

pub use keybox::{IssueGarageSvidResponse, KeyboxClient, KeyboxError};
pub use lifecycle::{GarageLifecycle, LifecycleError};
pub use service::{CreateGarageInput, ExtendTtlInput, GarageService, GarageServiceError};

use std::sync::LazyLock;

/// Default TTL in seconds, configurable via `MOTO_CLUB_DEFAULT_TTL_SECONDS` (default: 14400 = 4h).
pub static DEFAULT_TTL_SECONDS: LazyLock<i32> = LazyLock::new(|| {
    std::env::var("MOTO_CLUB_DEFAULT_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(14400)
});

/// Maximum TTL in seconds, configurable via `MOTO_CLUB_MAX_TTL_SECONDS` (default: 172800 = 48h).
pub static MAX_TTL_SECONDS: LazyLock<i32> = LazyLock::new(|| {
    std::env::var("MOTO_CLUB_MAX_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(172_800)
});

/// Minimum TTL in seconds, configurable via `MOTO_CLUB_MIN_TTL_SECONDS` (default: 300 = 5min).
pub static MIN_TTL_SECONDS: LazyLock<i32> = LazyLock::new(|| {
    std::env::var("MOTO_CLUB_MIN_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
});

/// Default dev container image.
pub const DEFAULT_IMAGE: &str = "ghcr.io/nhalm/moto-dev:latest";
