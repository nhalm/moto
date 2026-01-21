//! Garage subcommands: list, open, close, logs.

use clap::{Args, Subcommand};
use futures_util::StreamExt;
use moto_club_types::GarageId;
use moto_garage::GarageClient;
use serde::Serialize;
use std::io::{self, Write};

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

        /// Time-to-live (default: 4h, max: 48h). Format: <number><unit> where unit is m, h, or d.
        #[arg(long, default_value = "4h")]
        ttl: String,
    },

    /// Close an existing garage
    Close {
        /// ID of the garage to close (full UUID or short prefix)
        id: String,
    },

    /// View logs from a garage
    Logs {
        /// Name of the garage
        name: String,

        /// Stream logs continuously (Ctrl+C to stop)
        #[arg(long, short = 'f')]
        follow: bool,

        /// Show last n lines (default: 100)
        #[arg(long, short = 'n', default_value = "100")]
        tail: i64,

        /// Show logs from last duration (e.g., 5m, 1h)
        #[arg(long)]
        since: Option<String>,
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

/// JSON output for garage logs
#[derive(Serialize)]
struct GarageLogsJson {
    name: String,
    logs: String,
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
        GarageAction::Open { name, owner, ttl } => {
            let owner_ref = owner.as_deref();
            let ttl_seconds = parse_ttl(&ttl)?;
            if !flags.quiet && !flags.json {
                println!("Opening garage '{name}'...");
            }
            let garage = client.open(&name, owner_ref, Some(ttl_seconds)).await?;
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
                if let Some(expires) = garage.expires_at {
                    println!("  Expires:   {}", expires.format("%Y-%m-%d %H:%M:%S UTC"));
                }
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
        GarageAction::Logs {
            name,
            follow,
            tail,
            since,
        } => {
            let since_seconds = since.as_deref().map(parse_duration).transpose()?;

            if follow {
                // Streaming mode - --json is not supported with --follow
                if flags.json {
                    return Err("JSON output is not supported with --follow".into());
                }

                if !flags.quiet {
                    eprintln!("Streaming logs from '{name}'... (Ctrl+C to stop)");
                }

                let mut stream = client.logs_stream(&name, Some(tail), since_seconds).await?;
                let stdout = io::stdout();
                let mut handle = stdout.lock();

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(line) => {
                            write!(handle, "{line}")?;
                            handle.flush()?;
                        }
                        Err(e) => {
                            eprintln!("Error reading logs: {e}");
                            break;
                        }
                    }
                }
            } else {
                // Non-streaming mode
                let logs = client.logs(&name, Some(tail), since_seconds).await?;

                if flags.json {
                    let json = GarageLogsJson {
                        name: name.clone(),
                        logs: logs.clone(),
                    };
                    println!("{}", serde_json::to_string_pretty(&json)?);
                } else if logs.is_empty() {
                    if !flags.quiet {
                        println!("No logs found for garage '{name}'.");
                    }
                } else {
                    print!("{logs}");
                    if !logs.ends_with('\n') {
                        println!();
                    }
                }
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

/// Parse a duration string like "5m", "1h", "2d" into seconds.
fn parse_duration(s: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| format!("invalid duration number: {num_str}"))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(format!("invalid duration unit: {unit} (use s, m, h, or d)").into()),
    };

    Ok(num * multiplier)
}

/// Parse a TTL duration string with validation (max 48h).
fn parse_ttl(s: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let seconds = parse_duration(s)?;
    const MAX_TTL_SECONDS: i64 = 48 * 3600; // 48 hours

    if seconds <= 0 {
        return Err("TTL must be positive".into());
    }
    if seconds > MAX_TTL_SECONDS {
        return Err(format!("TTL exceeds maximum of 48h (got {s})").into());
    }

    Ok(seconds)
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
