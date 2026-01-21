//! Garage subcommands: list, open, close.

use clap::{Args, Subcommand};
use moto_club_types::GarageId;
use moto_garage::GarageClient;
use serde::Serialize;

use crate::cli::GlobalFlags;

/// Garage command and subcommands
#[derive(Args)]
pub struct GarageCommand {
    #[command(subcommand)]
    pub action: GarageAction,
}

/// Available garage actions
#[derive(Subcommand)]
pub enum GarageAction {
    /// List all garages
    List,

    /// Open a new garage
    Open {
        /// Name for the garage (human-friendly identifier)
        name: String,

        /// Owner of the garage (defaults to current user)
        #[arg(short, long)]
        owner: Option<String>,
    },

    /// Close an existing garage
    Close {
        /// ID of the garage to close (full UUID or short prefix)
        id: String,
    },
}

/// JSON output for garage list
#[derive(Serialize)]
struct GarageListJson {
    garages: Vec<GarageJson>,
}

/// JSON representation of a garage
#[derive(Serialize)]
struct GarageJson {
    name: String,
    id: String,
    status: String,
    namespace: String,
}

/// JSON output for garage open
#[derive(Serialize)]
struct GarageOpenJson {
    name: String,
    id: String,
    status: String,
    namespace: String,
}

/// JSON output for garage close
#[derive(Serialize)]
struct GarageCloseJson {
    name: String,
    status: String,
}

/// Run the garage command
pub async fn run(cmd: GarageCommand, flags: &GlobalFlags) -> Result<(), Box<dyn std::error::Error>> {
    let client = GarageClient::local().await?;

    match cmd.action {
        GarageAction::List => {
            let garages = client.list().await?;
            if flags.json {
                let json = GarageListJson {
                    garages: garages
                        .iter()
                        .map(|g| GarageJson {
                            name: g.name.clone(),
                            id: g.id.to_string(),
                            status: g.state.to_string(),
                            namespace: g.namespace.clone(),
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if garages.is_empty() {
                if !flags.quiet {
                    println!("No garages found.");
                }
            } else {
                println!("{:<12} {:<20} {:<12} {}", "ID", "NAME", "STATE", "NAMESPACE");
                println!("{}", "-".repeat(60));
                for g in garages {
                    println!(
                        "{:<12} {:<20} {:<12} {}",
                        g.id.short(),
                        truncate(&g.name, 20),
                        g.state,
                        g.namespace
                    );
                }
            }
        }
        GarageAction::Open { name, owner } => {
            let owner_ref = owner.as_deref();
            if !flags.quiet && !flags.json {
                println!("Opening garage '{name}'...");
            }
            let garage = client.open(&name, owner_ref).await?;
            if flags.json {
                let json = GarageOpenJson {
                    name: garage.name.clone(),
                    id: garage.id.to_string(),
                    status: garage.state.to_string(),
                    namespace: garage.namespace.clone(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("Garage opened:");
                println!("  ID:        {}", garage.id);
                println!("  Name:      {}", garage.name);
                println!("  Namespace: {}", garage.namespace);
                println!("  State:     {}", garage.state);
            }
        }
        GarageAction::Close { id } => {
            let garage_id = resolve_garage_id(&client, &id).await?;
            let garages = client.list().await?;
            let name = garages
                .iter()
                .find(|g| g.id == garage_id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| garage_id.short());
            if !flags.quiet && !flags.json {
                println!("Closing garage {}...", garage_id.short());
            }
            client.close(&garage_id).await?;
            if flags.json {
                let json = GarageCloseJson {
                    name,
                    status: "closed".to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Garage closed.");
            }
        }
    }

    Ok(())
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Resolve a garage ID from a full UUID or short prefix
async fn resolve_garage_id(
    client: &GarageClient,
    id_str: &str,
) -> Result<GarageId, Box<dyn std::error::Error>> {
    // Try parsing as full UUID first
    if let Ok(id) = id_str.parse::<GarageId>() {
        return Ok(id);
    }

    // Otherwise, treat as a prefix and search
    let garages = client.list().await?;
    let matches: Vec<_> = garages
        .iter()
        .filter(|g| g.id.to_string().starts_with(id_str) || g.id.short().starts_with(id_str))
        .collect();

    match matches.len() {
        0 => Err(format!("No garage found matching '{id_str}'").into()),
        1 => Ok(matches[0].id.clone()),
        _ => {
            let ids: Vec<_> = matches.iter().map(|g| g.id.short()).collect();
            Err(format!("Ambiguous ID '{id_str}', matches: {}", ids.join(", ")).into())
        }
    }
}
