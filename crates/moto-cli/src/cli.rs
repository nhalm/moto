//! CLI argument definitions using clap.

use clap::{Parser, Subcommand};

use crate::commands::garage::GarageCommand;
use crate::config::{ColorMode, Config};

/// Global flags that apply to all commands.
#[derive(Clone, Debug, Default)]
pub struct GlobalFlags {
    /// Output in JSON format
    pub json: bool,
    /// Verbosity level (0 = normal, 1+ = verbose)
    pub verbose: u8,
    /// Suppress non-essential output
    pub quiet: bool,
    /// Kubectl context to use
    pub context: Option<String>,
    /// Effective color mode (considers MOTO_NO_COLOR env var and config)
    pub color: ColorMode,
    /// Loaded configuration
    pub config: Config,
}

/// Moto - fintech motorcycle for tokenization, proxy, payments, and lending.
#[derive(Parser)]
#[command(name = "moto")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Output in JSON format
    #[arg(short = 'j', long, global = true)]
    pub json: bool,

    /// Increase output verbosity
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress non-essential output
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Override kubectl context
    #[arg(short = 'c', long, global = true)]
    pub context: Option<String>,

    /// The command to run
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Extract global flags from the CLI arguments with loaded config.
    pub fn global_flags(&self, config: Config) -> GlobalFlags {
        let color = ColorMode::effective(config.output.color);
        GlobalFlags {
            json: self.json || std::env::var("MOTO_JSON").is_ok(),
            verbose: self.verbose,
            quiet: self.quiet,
            context: self.context.clone(),
            color,
            config,
        }
    }
}

/// Top-level commands
#[derive(Subcommand)]
pub enum Command {
    /// Manage development garages (isolated environments)
    Garage(GarageCommand),
}
