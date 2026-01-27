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
        Command::IssueDevSvid(cmd) => commands::issue_dev_svid::run(cmd),
        Command::Set(cmd) => run_async(commands::secrets::run_set(cmd)),
        Command::Get(cmd) => run_async(commands::secrets::run_get(cmd)),
        Command::List(cmd) => run_async(commands::secrets::run_list(cmd)),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(i32::from(e.exit_code));
    }
}

/// Run an async function using tokio runtime.
fn run_async<F>(future: F) -> error::Result<()>
where
    F: std::future::Future<Output = error::Result<()>>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
        .block_on(future)
}
