//! Keybox CLI - secrets management and key generation.

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;
mod error;

use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .init();

    let result = match &cli.command {
        Command::Init(cmd) => commands::init::run(cmd),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(i32::from(e.exit_code));
    }
}
