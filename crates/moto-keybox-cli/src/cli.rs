//! CLI argument definitions using clap.

use clap::{Parser, Subcommand};

use crate::commands::init::InitCommand;

/// Keybox CLI - secrets management and key generation.
#[derive(Parser)]
#[command(name = "moto-keybox")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Increase output verbosity
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// The command to run
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands
#[derive(Subcommand)]
pub enum Command {
    /// Generate KEK and SVID signing key for keybox server
    Init(InitCommand),
}
