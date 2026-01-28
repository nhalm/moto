//! Bike subcommands: build, deploy, list, logs.

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::bike::discover_bike;
use crate::cli::GlobalFlags;
use crate::error::Result;

/// Bike command and subcommands
#[derive(Args)]
pub struct BikeCommand {
    #[command(subcommand)]
    pub action: BikeAction,
}

/// Available bike actions
#[derive(Subcommand)]
pub enum BikeAction {
    /// Build container image from bike.toml
    Build {
        /// Override image tag (default: git sha)
        #[arg(long)]
        tag: Option<String>,

        /// Push to registry after build
        #[arg(long)]
        push: bool,
    },
}

/// JSON output for bike build
#[derive(Serialize)]
struct BikeBuildJson {
    name: String,
    image: String,
    pushed: bool,
}

/// Run the bike command
pub async fn run(cmd: BikeCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        BikeAction::Build { tag: _, push: _ } => {
            // Discover bike.toml - this validates the file exists and is valid
            let (bike_path, config) = discover_bike()?;

            if flags.json {
                // For now, just output what we found
                let json = BikeBuildJson {
                    name: config.name.clone(),
                    image: format!("{}:pending", config.name),
                    pushed: false,
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Found bike.toml at {}", bike_path.display());
                println!("  Name: {}", config.name);
                println!("  Replicas: {}", config.deploy.replicas);
                println!("  Port: {}", config.deploy.port);
                println!();
                println!("Note: Nix build wrapper not yet implemented.");
            }
        }
    }

    Ok(())
}
