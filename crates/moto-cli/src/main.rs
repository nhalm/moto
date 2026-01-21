//! Moto CLI - the Bars (handlebars for steering).
//!
//! This is the main entry point for the `moto` command-line tool.

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;
mod config;

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
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)))
        .init();

    let result = match cli.command {
        Command::Garage(cmd) => commands::garage::run(cmd, &flags).await,
    };

    if let Err(e) = result {
        if flags.json {
            eprintln!(r#"{{"error": "{}"}}"#, e.to_string().replace('"', "\\\""));
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
