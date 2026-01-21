//! Moto CLI - the Bars (handlebars for steering).
//!
//! This is the main entry point for the `moto` command-line tool.

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;

use cli::{Cli, Command};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Command::Garage(cmd) => commands::garage::run(cmd).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
