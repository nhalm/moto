//! Moto CLI - the Bars (handlebars for steering).
//!
//! This is the main entry point for the `moto` command-line tool.

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod bike;
mod cli;
mod commands;
mod config;
mod error;
mod names;

use cli::{Cli, Command};
use config::Config;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = Config::load();
    let flags = cli.global_flags(config);

    // Initialize tracing based on verbosity
    let filter = if flags.verbose > 0 {
        match flags.verbose {
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    } else {
        "warn"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .init();

    let result = match cli.command {
        Command::Bike(cmd) => commands::bike::run(cmd, &flags).await,
        Command::Cluster(cmd) => commands::cluster::run(cmd, &flags).await,
        Command::Dev(cmd) => commands::dev::run(cmd, &flags).await,
        Command::Garage(cmd) => Box::pin(commands::garage::run(cmd, &flags)).await,
    };

    if let Err(e) = result {
        if flags.json {
            eprintln!(r#"{{"error": "{}"}}"#, e.to_string().replace('"', "\\\""));
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(i32::from(e.exit_code));
    }
}
