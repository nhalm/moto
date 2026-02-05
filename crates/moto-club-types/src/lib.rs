//! Shared types between CLI and server.
//!
//! This crate contains types that need to be shared between `moto-cli` and `moto-club`:
//! - `GarageId` for garage identification
//! - API request/response types (future)
//!
//! NOTE: `GarageStatus` is defined in `moto-club-db/src/models.rs` (the single source of truth
//! for garage status per spec v1.6). Crates needing the status enum should depend on `moto-club-db`.

mod garage;

pub use garage::GarageId;
