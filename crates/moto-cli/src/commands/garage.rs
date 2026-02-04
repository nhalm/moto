//! Garage subcommands: list, open, close, logs, enter, extend.
//!
//! The CLI talks directly to moto-club API for all garage operations.
//! Local mode (direct K8s access) is deprecated per project-structure.md v1.2.

use clap::{Args, Subcommand};
use moto_cli_wgtunnel::{
    ClientError, ConsoleProgress, CreateGarageRequest, EnterConfig, EnterError, MotoClubClient,
    MotoClubConfig, TunnelManager, enter_garage,
};
use serde::Serialize;
use std::io::{self, Write};

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

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
        /// Owner of the garage (defaults to current user)
        #[arg(short, long)]
        owner: Option<String>,

        /// Git branch to work on (default: current branch)
        #[arg(short, long)]
        branch: Option<String>,

        /// Time-to-live (max: 48h). Format: <number><unit> where unit is m, h, or d.
        /// Default: 4h or config file setting.
        #[arg(long)]
        ttl: Option<String>,

        /// Engine to work on (default: current directory name)
        #[arg(short, long)]
        engine: Option<String>,

        /// Include PostgreSQL database (postgres:16)
        #[arg(long)]
        with_postgres: bool,

        /// Include Redis cache (redis:7)
        #[arg(long)]
        with_redis: bool,
    },

    /// Connect to a garage terminal session
    Enter {
        /// Name of the garage to enter
        name: String,
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

    /// Extend a garage's TTL
    Extend {
        /// Name of the garage to extend
        name: String,

        /// Time to add to current TTL (e.g., 2h, 30m)
        #[arg(long, default_value = "2h")]
        ttl: String,
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

/// JSON output for garage enter
#[derive(Serialize)]
struct GarageEnterJson {
    name: String,
    session_id: String,
    client_ip: String,
    garage_ip: String,
    path_type: String,
    path_detail: String,
}

/// JSON output for garage extend
#[derive(Serialize)]
struct GarageExtendJson {
    name: String,
    expires_at: String,
    ttl_remaining_seconds: i64,
}

/// Create a moto-club client from configuration.
fn create_client(_flags: &GlobalFlags) -> Result<MotoClubClient> {
    // Get base URL from config or environment
    let base_url =
        std::env::var("MOTO_CLUB_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Get owner from environment variable
    // Per moto-club.md spec: MOTO_USER is required for local dev
    let owner = std::env::var("MOTO_USER").map_err(|_| {
        CliError::invalid_input(
            "MOTO_USER environment variable is required.\n\n\
             Set MOTO_USER to your username, e.g.:\n\
             export MOTO_USER=\"your-username\"",
        )
    })?;

    let config = MotoClubConfig::new(base_url, owner);
    MotoClubClient::new(config)
        .map_err(|e| CliError::general(format!("failed to create client: {e}")))
}

/// Convert client error to CLI error.
fn client_error_to_cli_error(e: ClientError) -> CliError {
    match e {
        ClientError::GarageNotFound(msg) => CliError::not_found(msg),
        ClientError::NotAuthorized(msg) => CliError::general(format!("not authorized: {msg}")),
        ClientError::Unreachable { url, reason } => CliError::general(format!(
            "moto-club unreachable at {url}: {reason}\n\n\
             Make sure moto-club is running.\n\
             Try: moto cluster status"
        )),
        ClientError::Server { code, message } => CliError::general(format!("{code}: {message}")),
        _ => CliError::general(e.to_string()),
    }
}

/// Run the garage command
pub async fn run(cmd: GarageCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        GarageAction::List => {
            let client = create_client(flags)?;
            let now = chrono::Utc::now();

            let response = client
                .list_garages()
                .await
                .map_err(client_error_to_cli_error)?;

            if flags.json {
                let json = GarageListJson {
                    garages: response
                        .garages
                        .iter()
                        .map(|g| {
                            let created_at = chrono::DateTime::parse_from_rfc3339(&g.created_at)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .unwrap_or(now);
                            let age_seconds = (now - created_at).num_seconds();

                            let ttl_remaining_seconds =
                                chrono::DateTime::parse_from_rfc3339(&g.expires_at)
                                    .ok()
                                    .map(|exp| {
                                        let remaining =
                                            (exp.with_timezone(&chrono::Utc) - now).num_seconds();
                                        remaining.max(0)
                                    });

                            GarageJson {
                                name: g.name.clone(),
                                status: g.status.clone(),
                                age_seconds,
                                ttl_remaining_seconds,
                                engine: g.engine.clone(),
                            }
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if response.garages.is_empty() {
                if !flags.quiet {
                    println!("No garages found.");
                }
            } else {
                println!(
                    "{:<16} {:<12} {:<8} {:<10} {}",
                    "NAME", "STATUS", "AGE", "TTL", "ENGINE"
                );
                for g in response.garages {
                    let created_at = chrono::DateTime::parse_from_rfc3339(&g.created_at)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or(now);
                    let age = format_duration((now - created_at).num_seconds());

                    let ttl = chrono::DateTime::parse_from_rfc3339(&g.expires_at)
                        .ok()
                        .map(|exp| {
                            let remaining = (exp.with_timezone(&chrono::Utc) - now).num_seconds();
                            if remaining <= 0 {
                                "expired".to_string()
                            } else {
                                format_duration(remaining)
                            }
                        })
                        .unwrap_or_else(|| "-".to_string());

                    let engine = g.engine.as_deref().unwrap_or("-");
                    println!(
                        "{:<16} {:<12} {:<8} {:<10} {}",
                        truncate(&g.name, 16),
                        g.status,
                        age,
                        ttl,
                        engine
                    );
                }
            }
        }

        GarageAction::Open {
            owner: _owner,
            branch,
            ttl,
            engine,
            with_postgres,
            with_redis,
        } => {
            let client = create_client(flags)?;
            let name = crate::names::generate();

            // Use CLI flag, then config default, then hardcoded default
            let ttl_str = ttl
                .as_deref()
                .or(flags.config.garage.ttl.as_deref())
                .unwrap_or("4h");
            let ttl_seconds = parse_ttl(ttl_str)?;

            if !flags.quiet && !flags.json {
                println!("Opening garage...");
            }

            let request = CreateGarageRequest {
                name: Some(name.clone()),
                branch: branch.clone(),
                ttl_seconds: Some(ttl_seconds),
                image: None,
                engine: engine.clone(),
                with_postgres: if with_postgres { Some(true) } else { None },
                with_redis: if with_redis { Some(true) } else { None },
            };

            let garage = client
                .create_garage(&request)
                .await
                .map_err(client_error_to_cli_error)?;

            if flags.json {
                let json = GarageOpenJson {
                    name: garage.name.clone(),
                    engine: garage.engine.clone(),
                    ttl_seconds,
                    status: garage.status.clone(),
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

        GarageAction::Enter { name } => {
            let client = create_client(flags)?;

            // Check if garage exists first
            client
                .get_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

            if !flags.quiet && !flags.json {
                eprintln!("Connecting to garage {name}...");
            }

            // Initialize tunnel manager
            let manager = TunnelManager::new()
                .await
                .map_err(|e| CliError::general(format!("failed to initialize tunnel: {e}")))?;

            // Configure enter
            let config = EnterConfig::default();
            let progress = ConsoleProgress::new(flags.quiet);

            // Enter the garage
            let session = enter_garage(&manager, &name, config, &progress)
                .await
                .map_err(|e| match e {
                    EnterError::GarageNotFound(_) => CliError::not_found(format!(
                        "Garage '{}' not found.\n\nTry: moto garage list",
                        name
                    )),
                    EnterError::NotAuthorized(_) => CliError::general(format!(
                        "Not authorized to access garage '{}'.\n\nCheck your permissions.",
                        name
                    )),
                    EnterError::ConnectionFailed(msg) => CliError::general(format!(
                        "Connection failed: {msg}\n\nTry: moto garage logs {name}"
                    )),
                    _ => CliError::general(e.to_string()),
                })?;

            if flags.json {
                // JSON mode: output session info without connecting
                let json = GarageEnterJson {
                    name: session.garage_name().to_string(),
                    session_id: session.session_id().to_string(),
                    client_ip: "".to_string(), // Not exposed on session handle
                    garage_ip: session.garage_ip().to_string(),
                    path_type: "derp".to_string(), // Default for now
                    path_detail: "primary".to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                // Interactive mode: connect to ttyd terminal
                if !flags.quiet {
                    eprintln!("  Opening terminal session... done");
                    eprintln!();
                }

                // Connect to ttyd - this blocks until the terminal session ends
                session.connect_ttyd().await.map_err(|e| match e {
                    EnterError::TtydFailed(msg) => CliError::general(format!(
                        "Terminal connection failed: {msg}\n\n\
                         This may happen if the garage is still starting up.\n\
                         Try: moto garage logs {name}"
                    )),
                    _ => CliError::general(e.to_string()),
                })?;
            }
        }

        GarageAction::Close { name, force } => {
            let client = create_client(flags)?;

            // Check if garage exists first
            client
                .get_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

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

            client
                .close_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

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
            tail: _tail,
            since: _since,
        } => {
            // Logs endpoint not yet implemented in HTTP client.
            // Per spec, WebSocket streaming will handle this in a future version.
            // For now, show an informative error.
            if follow {
                return Err(CliError::general(
                    "Log streaming via --follow is not yet supported.\n\n\
                     Use kubectl logs to view garage logs directly:\n\
                     kubectl logs -n moto-garage-<id> garage -f",
                ));
            }

            return Err(CliError::general(format!(
                "Log viewing is not yet supported via moto-club API.\n\n\
                 Use kubectl logs to view garage '{}' logs:\n\
                 kubectl logs -n moto-garage-<id> garage",
                name
            )));
        }

        GarageAction::Extend { name, ttl } => {
            let client = create_client(flags)?;

            // Parse TTL extension
            let seconds = parse_duration(&ttl)?;
            if seconds <= 0 {
                return Err(CliError::invalid_input("TTL extension must be positive"));
            }

            if !flags.quiet && !flags.json {
                println!("Extending garage '{name}' TTL by {ttl}...");
            }

            let response = client
                .extend_garage(&name, seconds)
                .await
                .map_err(|e| match e {
                    ClientError::GarageNotFound(_) => CliError::not_found(format!(
                        "Garage '{}' not found.\n\nTry: moto garage list",
                        name
                    )),
                    ClientError::Server { code, message } if code == "GARAGE_EXPIRED" => {
                        CliError::general(format!(
                            "Garage '{}' has expired and cannot be extended.",
                            name
                        ))
                    }
                    ClientError::Server { code, message } if code == "INVALID_TTL" => {
                        CliError::invalid_input(message)
                    }
                    _ => client_error_to_cli_error(e),
                })?;

            if flags.json {
                let json = GarageExtendJson {
                    name: name.clone(),
                    expires_at: response.expires_at.clone(),
                    ttl_remaining_seconds: response.ttl_remaining_seconds,
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                let ttl_formatted = format_duration(response.ttl_remaining_seconds);
                println!("Garage TTL extended.");
                println!("  New TTL: {ttl_formatted}");
                println!("  Expires: {}", response.expires_at);
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
fn parse_duration(s: &str) -> Result<i64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(CliError::invalid_input("empty duration"));
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| CliError::invalid_input(format!("invalid duration number: {num_str}")))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(CliError::invalid_input(format!(
                "invalid duration unit: {unit} (use s, m, h, or d)"
            )));
        }
    };

    Ok(num * multiplier)
}

/// Parse a TTL duration string with validation (max 48h).
fn parse_ttl(s: &str) -> Result<i64> {
    let seconds = parse_duration(s)?;
    const MAX_TTL_SECONDS: i64 = 48 * 3600; // 48 hours

    if seconds <= 0 {
        return Err(CliError::invalid_input("TTL must be positive"));
    }
    if seconds > MAX_TTL_SECONDS {
        return Err(CliError::invalid_input(format!(
            "TTL exceeds maximum of 48h (got {s})"
        )));
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
