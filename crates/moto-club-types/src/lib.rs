//! Shared types between CLI and server.
//!
//! This crate contains types that need to be shared between `moto-cli` and `moto-club`:
//! - `GarageId`, `GarageState`, `GarageInfo` for garage management
//! - API request/response types (future)

mod garage;

pub use garage::{GarageId, GarageInfo, GarageState};
