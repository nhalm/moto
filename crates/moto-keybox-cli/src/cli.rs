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
    /// Set a secret value
    Set(SetArgs),
    /// Get a secret value
    Get(GetArgs),
    /// List secrets in a scope
    List(ListArgs),
}

/// Arguments for the `set` command.
#[derive(clap::Args, Debug)]
pub struct SetArgs {
    /// Keybox server URL
    #[arg(long, default_value = "http://localhost:8080", env = "MOTO_KEYBOX_URL")]
    pub url: String,

    /// Authentication token (SVID)
    #[arg(long, env = "MOTO_KEYBOX_TOKEN")]
    pub token: String,

    /// Secret scope (global, service, instance)
    pub scope: String,

    /// Secret name (e.g., "ai/anthropic" for global, "tokenization/db-password" for service)
    pub name: String,

    /// Secret value (omit to read from stdin)
    pub value: Option<String>,

    /// Read value from stdin
    #[arg(long)]
    pub stdin: bool,
}

/// Arguments for the `get` command.
#[derive(clap::Args, Debug)]
pub struct GetArgs {
    /// Keybox server URL
    #[arg(long, default_value = "http://localhost:8080", env = "MOTO_KEYBOX_URL")]
    pub url: String,

    /// Authentication token (SVID)
    #[arg(long, env = "MOTO_KEYBOX_TOKEN")]
    pub token: String,

    /// Secret scope (global, service, instance)
    pub scope: String,

    /// Secret name
    pub name: String,
}

/// Arguments for the `list` command.
#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Keybox server URL
    #[arg(long, default_value = "http://localhost:8080", env = "MOTO_KEYBOX_URL")]
    pub url: String,

    /// Authentication token (SVID)
    #[arg(long, env = "MOTO_KEYBOX_TOKEN")]
    pub token: String,

    /// Secret scope (global, service, instance)
    pub scope: String,

    /// For service scope: filter by service name
    #[arg(long)]
    pub service: Option<String>,

    /// For instance scope: filter by instance ID
    #[arg(long)]
    pub instance: Option<String>,
}
