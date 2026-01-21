//! Garage subcommands: list, open, close, logs.

use clap::{Args, Subcommand};
use futures_util::StreamExt;
use moto_garage::GarageClient;
use moto_k8s::K8sClient;
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
    List {
        /// Filter by kubectl context (use "all" for all contexts)
        #[arg(long)]
        context: Option<String>,
    },

    /// Open a new garage
    Open {
        /// Owner of the garage (defaults to current user)
        #[arg(short, long)]
        owner: Option<String>,

        /// Time-to-live (max: 48h). Format: <number><unit> where unit is m, h, or d.
        /// Default: 4h or config file setting.
        #[arg(long)]
        ttl: Option<String>,

        /// Engine to work on (default: current directory name)
        #[arg(short, long)]
        engine: Option<String>,
    },

    /// Close an existing garage
    Close {
        /// Name of the garage to close
        name: String,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
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

/// JSON representation of a garage (matches spec)
#[derive(Serialize)]
struct GarageJson {
    name: String,
    status: String,
    age_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_remaining_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}

/// JSON output for garage open (matches spec)
#[derive(Serialize)]
struct GarageOpenJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    engine: Option<String>,
    ttl_seconds: i64,
    status: String,
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

/// Garage info with context name for multi-context listing.
struct GarageWithContext {
    garage: moto_club_types::GarageInfo,
    context: Option<String>,
}

/// Run the garage command
pub async fn run(cmd: GarageCommand, flags: &GlobalFlags) -> Result<(), Box<dyn std::error::Error>> {
    match cmd.action {
        GarageAction::List { context } => {
            let now = chrono::Utc::now();
            let show_context_column = context.as_deref() == Some("all");

            // Collect garages from the appropriate context(s)
            let garages_with_context: Vec<GarageWithContext> = if context.as_deref() == Some("all") {
                // List from all contexts
                let contexts = K8sClient::list_contexts()?;
                let mut all_garages = Vec::new();
                for ctx_name in contexts {
                    match GarageClient::local_with_context(&ctx_name).await {
                        Ok(client) => {
                            if let Ok(garages) = client.list().await {
                                for g in garages {
                                    all_garages.push(GarageWithContext {
                                        garage: g,
                                        context: Some(ctx_name.clone()),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            // Skip contexts that fail to connect (e.g., cluster not available)
                            if flags.verbose > 0 {
                                eprintln!("Warning: could not connect to context '{ctx_name}': {e}");
                            }
                        }
                    }
                }
                all_garages
            } else if let Some(ctx_name) = &context {
                // List from specific context
                let client = GarageClient::local_with_context(ctx_name).await?;
                client
                    .list()
                    .await?
                    .into_iter()
                    .map(|g| GarageWithContext {
                        garage: g,
                        context: Some(ctx_name.clone()),
                    })
                    .collect()
            } else {
                // List from current/default context
                let client = GarageClient::local().await?;
                client
                    .list()
                    .await?
                    .into_iter()
                    .map(|g| GarageWithContext {
                        garage: g,
                        context: None,
                    })
                    .collect()
            };

            if flags.json {
                let json = GarageListJson {
                    garages: garages_with_context
                        .iter()
                        .map(|gwc| {
                            let g = &gwc.garage;
                            let age_seconds = (now - g.created_at).num_seconds();
                            let ttl_remaining_seconds = g.expires_at.map(|exp| {
                                let remaining = (exp - now).num_seconds();
                                remaining.max(0)
                            });
                            GarageJson {
                                name: g.name.clone(),
                                status: g.state.to_string(),
                                age_seconds,
                                ttl_remaining_seconds,
                                engine: g.engine.clone(),
                                context: gwc.context.clone(),
                            }
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if garages_with_context.is_empty() {
                if !flags.quiet {
                    println!("No garages found.");
                }
            } else if show_context_column {
                println!(
                    "{:<16} {:<10} {:<8} {:<10} {:<16} {}",
                    "NAME", "STATUS", "AGE", "TTL", "CONTEXT", "ENGINE"
                );
                for gwc in garages_with_context {
                    let g = &gwc.garage;
                    let age = format_duration((now - g.created_at).num_seconds());
                    let ttl = g
                        .expires_at
                        .map(|exp| {
                            let remaining = (exp - now).num_seconds();
                            if remaining <= 0 {
                                "expired".to_string()
                            } else {
                                format_duration(remaining)
                            }
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let engine = g.engine.as_deref().unwrap_or("-");
                    let ctx = gwc.context.as_deref().unwrap_or("-");
                    println!(
                        "{:<16} {:<10} {:<8} {:<10} {:<16} {}",
                        truncate(&g.name, 16),
                        g.state,
                        age,
                        ttl,
                        truncate(ctx, 16),
                        engine
                    );
                }
            } else {
                println!(
                    "{:<16} {:<10} {:<8} {:<10} {}",
                    "NAME", "STATUS", "AGE", "TTL", "ENGINE"
                );
                for gwc in garages_with_context {
                    let g = &gwc.garage;
                    let age = format_duration((now - g.created_at).num_seconds());
                    let ttl = g
                        .expires_at
                        .map(|exp| {
                            let remaining = (exp - now).num_seconds();
                            if remaining <= 0 {
                                "expired".to_string()
                            } else {
                                format_duration(remaining)
                            }
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let engine = g.engine.as_deref().unwrap_or("-");
                    println!(
                        "{:<16} {:<10} {:<8} {:<10} {}",
                        truncate(&g.name, 16),
                        g.state,
                        age,
                        ttl,
                        engine
                    );
                }
            }
        }
        GarageAction::Open { owner, ttl, engine } => {
            let client = GarageClient::local().await?;
            let name = crate::names::generate();
            let owner_ref = owner.as_deref();
            let engine_ref = engine.as_deref();
            // Use CLI flag, then config default, then hardcoded default
            let ttl_str = ttl
                .as_deref()
                .or(flags.config.garage.ttl.as_deref())
                .unwrap_or("4h");
            let ttl_seconds = parse_ttl(ttl_str)?;
            if !flags.quiet && !flags.json {
                println!("Opening garage...");
            }
            let garage = client.open(&name, owner_ref, Some(ttl_seconds), engine_ref).await?;
            if flags.json {
                let json = GarageOpenJson {
                    name: garage.name.clone(),
                    engine: garage.engine.clone(),
                    ttl_seconds,
                    status: garage.state.to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("Created garage: {}", garage.name);
                if let Some(engine) = &garage.engine {
                    println!("  Engine: {engine}");
                }
                println!("  TTL: {ttl_str}");
                println!();
                println!("To connect: moto garage enter {}", garage.name);
            }
        }
        GarageAction::Close { name, force } => {
            let client = GarageClient::local().await?;
            // Check if garage exists first
            let garages = client.list().await?;
            if !garages.iter().any(|g| g.name == name) {
                return Err(format!("Garage '{}' not found.\n\nTry: moto garage list", name).into());
            }

            // Prompt for confirmation unless --force is used
            if !force && !flags.json {
                eprint!("Close garage '{name}'? [y/N] ");
                io::stderr().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    if !flags.quiet {
                        eprintln!("Aborted.");
                    }
                    return Ok(());
                }
            }

            if !flags.quiet && !flags.json {
                println!("Closing garage '{name}'...");
            }
            client.close_by_name(&name).await?;
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
            let client = GarageClient::local().await?;
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

/// Format a duration in seconds as a human-readable string (e.g., "2h15m", "45m", "3d").
fn format_duration(seconds: i64) -> String {
    if seconds < 0 {
        return "0s".to_string();
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        if hours > 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{hours}h")
        }
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3540), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h1m");
        assert_eq!(format_duration(8100), "2h15m");
        assert_eq!(format_duration(7200), "2h");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d1h");
        assert_eq!(format_duration(172800), "2d");
    }

    #[test]
    fn test_format_duration_negative() {
        assert_eq!(format_duration(-100), "0s");
    }
}
