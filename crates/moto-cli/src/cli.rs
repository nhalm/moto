//! CLI argument definitions using clap.

use clap::{Parser, Subcommand};

use crate::commands::garage::GarageCommand;

/// Moto - fintech motorcycle for tokenization, proxy, payments, and lending.
#[derive(Parser)]
#[command(name = "moto")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The command to run
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands
#[derive(Subcommand)]
pub enum Command {
    /// Manage development garages (isolated environments)
    Garage(GarageCommand),
}
