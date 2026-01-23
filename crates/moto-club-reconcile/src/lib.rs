//! K8s → DB reconciliation for moto-club.
//!
//! This crate provides poll-based reconciliation to keep the database
//! in sync with Kubernetes state. K8s is the source of truth.
//!
//! # Design
//!
//! The reconciler runs on a configurable interval (default: 30 seconds) and:
//!
//! 1. Lists all garage namespaces in K8s (label: `moto.dev/type=garage`)
//! 2. For each K8s namespace:
//!    - If garage exists in DB: update status to match pod status
//!    - If pod missing/terminated: mark garage as Terminated with reason `pod_lost`
//!    - If garage NOT in DB (orphan): log warning, optionally delete
//! 3. For each non-terminated garage in DB:
//!    - If no matching K8s namespace: mark as Terminated with reason `namespace_missing`
//!
//! **Note:** TTL enforcement is NOT done here. That's handled by moto-cron.
//!
//! # Example
//!
//! ```ignore
//! use moto_club_reconcile::{GarageReconciler, ReconcileConfig};
//! use moto_club_k8s::GarageK8s;
//! use moto_club_db::DbPool;
//!
//! let reconciler = GarageReconciler::new(db_pool, garage_k8s, ReconcileConfig::default());
//!
//! // Run one reconciliation cycle
//! let stats = reconciler.reconcile_once().await?;
//! println!("Updated: {}, Terminated: {}", stats.updated, stats.terminated);
//!
//! // Or run continuously in the background
//! reconciler.run().await;
//! ```

mod garage;

pub use garage::{GarageReconciler, ReconcileConfig, ReconcileError, ReconcileStats};
