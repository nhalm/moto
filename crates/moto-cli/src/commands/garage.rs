//! Garage subcommands: list, open, close.

use clap::{Args, Subcommand};
use moto_club_types::GarageId;
use moto_garage::GarageClient;

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

/// Run the garage command
pub async fn run(cmd: GarageCommand) -> Result<(), Box<dyn std::error::Error>> {
    let client = GarageClient::local().await?;

    match cmd.action {
        GarageAction::List => {
            let garages = client.list().await?;
            if garages.is_empty() {
                println!("No garages found.");
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
            println!("Opening garage '{name}'...");
            let garage = client.open(&name, owner_ref).await?;
            println!("Garage opened:");
            println!("  ID:        {}", garage.id);
            println!("  Name:      {}", garage.name);
            println!("  Namespace: {}", garage.namespace);
            println!("  State:     {}", garage.state);
        }
        GarageAction::Close { id } => {
            let garage_id = resolve_garage_id(&client, &id).await?;
            println!("Closing garage {}...", garage_id.short());
            client.close(&garage_id).await?;
            println!("Garage closed.");
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
