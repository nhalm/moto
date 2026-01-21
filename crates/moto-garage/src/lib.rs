//! Garage client library for moto.
//!
//! This crate provides the [`GarageClient`] for managing dev environments (garages).
//! It abstracts the difference between local mode (direct K8s access) and remote
//! mode (through moto-club server).
//!
//! # Modes
//!
//! - **Local**: Direct K8s access via kubeconfig. Use [`GarageClient::local()`].
//! - **Remote**: Through moto-club server. Use [`GarageClient::remote()`].
//!
//! # Example
//!
//! ```no_run
//! use moto_garage::GarageClient;
//!
//! # async fn example() -> moto_garage::Result<()> {
//! // Create a local client
//! let client = GarageClient::local().await?;
//!
//! // List existing garages
//! let garages = client.list().await?;
//! for g in &garages {
//!     println!("{}: {} ({})", g.id.short(), g.name, g.state);
//! }
//!
//! // Open a new garage with 4h TTL
//! let garage = client.open("my-project", Some("alice"), Some(4 * 3600), None).await?;
//! println!("Created garage: {}", garage.namespace);
//!
//! // Close the garage
//! client.close(&garage.id).await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod error;
mod mode;

pub use client::GarageClient;
pub use error::{Error, Result};
pub use mode::GarageMode;
pub use moto_k8s::LogStream;
